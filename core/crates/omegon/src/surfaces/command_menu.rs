//! Renderer-neutral slash-command menu projection.
//!
//! This projection sits above the raw command registry. It preserves the
//! registry's availability/safety metadata while adding menu-oriented source,
//! display, and badge fields that TUI autocomplete, `/help`, ACP, CLI, and
//! future web clients can share without reimplementing slash-menu policy.

use std::collections::HashSet;

use omegon_traits::{CommandAvailability, CommandDefinition, CommandSafety, CommandSafetyClass};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandMenuProjection {
    pub rows: Vec<CommandMenuRowProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandMenuRowProjection {
    pub name: String,
    pub command: String,
    pub description: String,
    pub subcommands: Vec<String>,
    pub source: CommandMenuSource,
    pub availability: CommandAvailabilityProjection,
    pub safety: CommandSafetyProjection,
    pub badges: Vec<String>,
    pub metadata: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandMenuSource {
    Builtin,
    Feature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandAvailabilityProjection {
    pub tui: bool,
    pub cli: bool,
    pub acp: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSafetyProjection {
    pub class: CommandSafetyClass,
    pub requires_confirmation: bool,
    pub prompt_injection_sensitive: bool,
}

impl CommandMenuProjection {
    pub fn matching(&self, input: &str) -> Vec<CommandMenuRowProjection> {
        let Some(input) = input.strip_prefix('/') else {
            return Vec::new();
        };
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        if parts.len() <= 1 {
            let prefix = parts.first().copied().unwrap_or("");
            self.rows
                .iter()
                .filter(|row| prefix.is_empty() || row.name.starts_with(prefix))
                .cloned()
                .collect()
        } else {
            let name = parts[0];
            let sub_prefix = parts.get(1).copied().unwrap_or("");
            if name == "auth" {
                let nested_parts: Vec<&str> = sub_prefix.splitn(2, ' ').collect();
                if matches!(nested_parts.first().copied(), Some("login" | "logout"))
                    && sub_prefix.contains(' ')
                {
                    let action = nested_parts[0];
                    let provider_prefix = nested_parts.get(1).copied().unwrap_or("");
                    return crate::auth::operator_auth_provider_ids()
                        .into_iter()
                        .filter(|provider| provider.starts_with(provider_prefix))
                        .map(|provider| {
                            let mut sub_row = self
                                .rows
                                .iter()
                                .find(|row| row.name == name)
                                .cloned()
                                .unwrap_or_else(|| CommandMenuRowProjection {
                                    name: name.to_string(),
                                    command: format!("/{name}"),
                                    description: String::new(),
                                    subcommands: Vec::new(),
                                    source: CommandMenuSource::Builtin,
                                    availability: CommandAvailabilityProjection {
                                        tui: true,
                                        cli: true,
                                        acp: true,
                                    },
                                    safety: CommandSafetyProjection {
                                        class: CommandSafetyClass::ExternalSideEffect,
                                        requires_confirmation: false,
                                        prompt_injection_sensitive: false,
                                    },
                                    badges: Vec::new(),
                                    metadata: Vec::new(),
                                });
                            sub_row.command = format!("/{name} {action} {provider}");
                            sub_row.description.clear();
                            sub_row.badges.clear();
                            sub_row.metadata.clear();
                            sub_row
                        })
                        .collect();
                }
            }
            let Some(row) = self.rows.iter().find(|row| row.name == name) else {
                return Vec::new();
            };
            row.subcommands
                .iter()
                .filter(|sub| sub.starts_with(sub_prefix))
                .map(|sub| {
                    let mut sub_row = row.clone();
                    sub_row.command = format!("/{name} {sub}");
                    sub_row.description.clear();
                    sub_row.badges.clear();
                    sub_row.metadata.clear();
                    sub_row
                })
                .collect()
        }
    }
}

pub fn command_menu_projection(
    builtin_commands: impl IntoIterator<Item = CommandDefinition>,
    feature_commands: impl IntoIterator<Item = CommandDefinition>,
    hidden_names: &[&str],
) -> CommandMenuProjection {
    let hidden: HashSet<&str> = hidden_names.iter().copied().collect();
    let mut seen = HashSet::new();
    let mut rows = Vec::new();

    for command in builtin_commands {
        if hidden.contains(command.name.as_str()) || !command.availability.tui {
            continue;
        }
        seen.insert(command.name.clone());
        rows.push(command_menu_row(command, CommandMenuSource::Builtin));
    }

    for command in feature_commands {
        if hidden.contains(command.name.as_str())
            || !command.availability.tui
            || !seen.insert(command.name.clone())
        {
            continue;
        }
        rows.push(command_menu_row(command, CommandMenuSource::Feature));
    }

    CommandMenuProjection { rows }
}

fn command_menu_row(
    definition: CommandDefinition,
    source: CommandMenuSource,
) -> CommandMenuRowProjection {
    let availability = availability_projection(definition.availability);
    let safety = safety_projection(definition.safety);
    let mut badges = vec![source.label().to_string(), safety.class_label().to_string()];
    if safety.requires_confirmation {
        badges.push("confirm".to_string());
    }
    if safety.prompt_injection_sensitive {
        badges.push("prompt".to_string());
    }
    if availability.cli || availability.acp {
        let mut surfaces = Vec::new();
        if availability.cli {
            surfaces.push("cli");
        }
        if availability.acp {
            surfaces.push("acp");
        }
        badges.push(surfaces.join("+"));
    }

    let metadata = command_metadata(&definition.name);

    CommandMenuRowProjection {
        command: format!("/{}", definition.name),
        name: definition.name,
        description: definition.description,
        subcommands: definition.subcommands,
        source,
        availability,
        safety,
        badges,
        metadata,
    }
}

fn command_metadata(name: &str) -> Vec<String> {
    match name {
        "think" | "context" => vec!["runtime until /profile save".to_string()],
        "profile" => vec!["save/apply runtime defaults".to_string()],
        "settings" => vec!["TUI settings modal".to_string()],
        _ => Vec::new(),
    }
}

fn availability_projection(availability: CommandAvailability) -> CommandAvailabilityProjection {
    CommandAvailabilityProjection {
        tui: availability.tui,
        cli: availability.cli,
        acp: availability.acp,
    }
}

fn safety_projection(safety: CommandSafety) -> CommandSafetyProjection {
    CommandSafetyProjection {
        class: safety.class,
        requires_confirmation: safety.requires_confirmation,
        prompt_injection_sensitive: safety.prompt_injection_sensitive,
    }
}

impl CommandMenuSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Feature => "feature",
        }
    }
}

impl CommandSafetyProjection {
    pub fn class_label(self) -> &'static str {
        match self.class {
            CommandSafetyClass::LocalOnly => "local",
            CommandSafetyClass::ReadOnly => "read",
            CommandSafetyClass::QueueMutation => "queue",
            CommandSafetyClass::StateChanging => "state",
            CommandSafetyClass::ExternalSideEffect => "external",
            CommandSafetyClass::Destructive => "destructive",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(
        name: &str,
        description: &str,
        subcommands: &[&str],
        availability: CommandAvailability,
        safety: CommandSafety,
    ) -> CommandDefinition {
        CommandDefinition {
            name: name.to_string(),
            description: description.to_string(),
            subcommands: subcommands.iter().map(|sub| sub.to_string()).collect(),
            availability,
            safety,
        }
    }

    #[test]
    fn projection_deduplicates_features_behind_builtins() {
        let projection = command_menu_projection(
            [command(
                "cleave",
                "builtin cleave",
                &["status"],
                CommandAvailability::TUI_ONLY,
                CommandSafety::LOCAL_ONLY,
            )],
            [command(
                "cleave",
                "feature cleave",
                &["run"],
                CommandAvailability::ALL,
                CommandSafety::READ_ONLY,
            )],
            &[],
        );

        assert_eq!(projection.rows.len(), 1);
        assert_eq!(projection.rows[0].description, "builtin cleave");
        assert_eq!(projection.rows[0].source, CommandMenuSource::Builtin);
    }

    #[test]
    fn projection_carries_safety_badges_for_feature_commands() {
        let projection = command_menu_projection(
            [],
            [command(
                "prompt",
                "run prompts",
                &["list"],
                CommandAvailability::ALL,
                CommandSafety::QUEUE_MUTATION,
            )],
            &[],
        );

        let row = &projection.rows[0];
        assert_eq!(row.command, "/prompt");
        assert!(row.badges.contains(&"feature".to_string()));
        assert!(row.badges.contains(&"queue".to_string()));
        assert!(row.badges.contains(&"prompt".to_string()));
        assert!(row.badges.contains(&"cli+acp".to_string()));
    }

    #[test]
    fn matching_projects_subcommand_rows() {
        let projection = command_menu_projection(
            [command(
                "context",
                "context lifecycle",
                &["status", "compact"],
                CommandAvailability::TUI_ONLY,
                CommandSafety::LOCAL_ONLY,
            )],
            [],
            &[],
        );

        let rows = projection.matching("/context st");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].command, "/context status");
        assert!(rows[0].description.is_empty());
    }

    #[test]
    fn matching_projects_nested_auth_provider_arguments() {
        let projection = command_menu_projection(
            [command(
                "auth",
                "authentication",
                &["status", "login", "logout"],
                CommandAvailability::ALL,
                CommandSafety::EXTERNAL_SIDE_EFFECT,
            )],
            [],
            &[],
        );

        let direct = projection.matching("/auth openai");
        assert!(
            direct.is_empty(),
            "provider ids must not be projected as direct /auth subcommands: {direct:?}"
        );

        let login = projection.matching("/auth login openai-c");
        assert_eq!(login.len(), 1);
        assert_eq!(login[0].command, "/auth login openai-codex");

        let logout = projection.matching("/auth logout github-c");
        assert_eq!(logout.len(), 1);
        assert_eq!(logout[0].command, "/auth logout github-copilot");
    }
}
