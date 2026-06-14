//! User-defined command aliases.
//!
//! First slice: explicit slash aliases that preview reusable prompts. Prompt IDs
//! remain data; command aliases are opt-in invocation surfaces.

use std::path::PathBuf;

use omegon_traits::{
    CommandAvailability, CommandDefinition, CommandResult, CommandSafety, CommandSafetyClass,
    Feature,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct UserCommandManifest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    target: String,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default)]
    availability: UserCommandAvailability,
    #[serde(default)]
    safety: UserCommandSafety,
}

#[derive(Debug, Clone, Deserialize)]
struct UserCommandAvailability {
    #[serde(default = "default_true")]
    tui: bool,
    #[serde(default = "default_true")]
    cli: bool,
    #[serde(default)]
    acp: bool,
}

impl Default for UserCommandAvailability {
    fn default() -> Self {
        Self {
            tui: true,
            cli: true,
            acp: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct UserCommandSafety {
    #[serde(default = "default_safety_class")]
    class: String,
    #[serde(default = "default_true")]
    requires_confirmation: bool,
    #[serde(default = "default_true")]
    prompt_injection_sensitive: bool,
}

impl Default for UserCommandSafety {
    fn default() -> Self {
        Self {
            class: default_safety_class(),
            requires_confirmation: true,
            prompt_injection_sensitive: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_mode() -> String {
    "preview".into()
}

fn default_safety_class() -> String {
    "queue_mutation".into()
}

#[derive(Debug, Clone)]
struct UserCommand {
    manifest: UserCommandManifest,
    path: PathBuf,
}

pub struct UserCommandFeature {
    commands: Vec<UserCommand>,
}

impl UserCommandFeature {
    pub fn load() -> Self {
        Self {
            commands: load_commands().unwrap_or_default(),
        }
    }

    #[cfg(test)]
    fn from_dir(root: &std::path::Path) -> Self {
        Self {
            commands: load_commands_from_dirs(&[root.join(".omegon/commands")]).unwrap_or_default(),
        }
    }
}

impl Feature for UserCommandFeature {
    fn name(&self) -> &str {
        "user_commands"
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        self.commands
            .iter()
            .filter_map(|cmd| command_definition(&cmd.manifest).ok())
            .collect()
    }

    fn handle_command(&mut self, name: &str, _args: &str) -> CommandResult {
        let Some(command) = self.commands.iter().find(|cmd| cmd.manifest.name == name) else {
            return CommandResult::NotHandled;
        };
        match preview_prompt_command(command) {
            Ok(output) => CommandResult::Display(output),
            Err(err) => CommandResult::Display(format!("/{name} failed: {err}")),
        }
    }
}

fn load_commands() -> anyhow::Result<Vec<UserCommand>> {
    let mut dirs = Vec::new();
    if let Ok(home) = crate::paths::omegon_home() {
        dirs.push(home.join("commands"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join(".omegon/commands"));
    }
    load_commands_from_dirs(&dirs)
}

fn load_commands_from_dirs(dirs: &[PathBuf]) -> anyhow::Result<Vec<UserCommand>> {
    let mut commands = Vec::new();
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let mut files: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
            .collect();
        files.sort();
        for path in files {
            let content = std::fs::read_to_string(&path)?;
            let manifest: UserCommandManifest = toml::from_str(&content)?;
            validate_manifest(&manifest)?;
            commands.push(UserCommand { manifest, path });
        }
    }
    Ok(commands)
}

fn validate_manifest(manifest: &UserCommandManifest) -> anyhow::Result<()> {
    validate_command_name(&manifest.name)?;
    if is_reserved_command(&manifest.name) {
        anyhow::bail!(
            "user command '{}' collides with a built-in command",
            manifest.name
        );
    }
    if manifest.mode != "preview" {
        anyhow::bail!("only preview mode is supported for user commands");
    }
    if !manifest.target.starts_with("prompt:") {
        anyhow::bail!("only prompt:<id> targets are supported for user commands");
    }
    let prompt_name = manifest.target.trim_start_matches("prompt:");
    crate::prompts::validate_name(prompt_name)?;
    Ok(())
}

fn validate_command_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("invalid user command name");
    }
    Ok(())
}

fn is_reserved_command(name: &str) -> bool {
    matches!(
        name,
        "help"
            | "exit"
            | "quit"
            | "model"
            | "context"
            | "plan"
            | "memory"
            | "skills"
            | "skill"
            | "prompt"
            | "prompts"
            | "extension"
            | "plugin"
            | "auth"
            | "cleave"
            | "delegate"
            | "secrets"
            | "vault"
            | "sandbox"
            | "ui"
    )
}

fn command_definition(manifest: &UserCommandManifest) -> anyhow::Result<CommandDefinition> {
    validate_manifest(manifest)?;
    Ok(CommandDefinition {
        name: manifest.name.clone(),
        description: manifest
            .description
            .clone()
            .unwrap_or_else(|| format!("Preview {}", manifest.target)),
        subcommands: vec![],
        availability: CommandAvailability {
            tui: manifest.availability.tui,
            cli: manifest.availability.cli,
            acp: manifest.availability.acp,
        },
        safety: CommandSafety {
            class: safety_class(&manifest.safety.class)?,
            requires_confirmation: manifest.safety.requires_confirmation,
            prompt_injection_sensitive: manifest.safety.prompt_injection_sensitive,
        },
    })
}

fn safety_class(class: &str) -> anyhow::Result<CommandSafetyClass> {
    Ok(match class {
        "local_only" => CommandSafetyClass::LocalOnly,
        "read_only" => CommandSafetyClass::ReadOnly,
        "queue_mutation" => CommandSafetyClass::QueueMutation,
        "state_changing" => CommandSafetyClass::StateChanging,
        "external_side_effect" => CommandSafetyClass::ExternalSideEffect,
        "destructive" => CommandSafetyClass::Destructive,
        other => anyhow::bail!("unknown command safety class '{other}'"),
    })
}

fn preview_prompt_command(command: &UserCommand) -> anyhow::Result<String> {
    let prompt_name = command.manifest.target.trim_start_matches("prompt:");
    let (_manifest, body, prompt_path) = crate::prompts::get_prompt(prompt_name)?;
    let safety = crate::prompts::safety_verdict(&body);
    if safety.is_blocked() {
        anyhow::bail!("target prompt is blocked by safety verdict: {safety:?}");
    }
    Ok(format!(
        "User command /{} -> prompt:{}\nCommand: {}\nPrompt: {}\nSafety: {:?}\n\n{}",
        command.manifest.name,
        prompt_name,
        command.path.display(),
        prompt_path.display(),
        safety,
        body
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_command_registers_prompt_preview_alias() {
        let dir = tempfile::tempdir().unwrap();
        let command_dir = dir.path().join(".omegon/commands");
        std::fs::create_dir_all(&command_dir).unwrap();
        std::fs::write(
            command_dir.join("start.toml"),
            r#"
name = "start"
description = "Preview init prompt"
target = "prompt:init"
mode = "preview"
"#,
        )
        .unwrap();

        let feature = UserCommandFeature::from_dir(dir.path());
        let commands = feature.commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "start");
        assert_eq!(commands[0].availability.tui, true);
        assert_eq!(commands[0].availability.cli, true);
        assert_eq!(commands[0].availability.acp, false);
        assert_eq!(commands[0].safety.class, CommandSafetyClass::QueueMutation);
    }

    #[test]
    fn user_command_dispatch_previews_target_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let command_dir = dir.path().join(".omegon/commands");
        std::fs::create_dir_all(&command_dir).unwrap();
        std::fs::write(
            command_dir.join("start.toml"),
            r#"
name = "start"
target = "prompt:init"
mode = "preview"
"#,
        )
        .unwrap();

        let mut feature = UserCommandFeature::from_dir(dir.path());
        match feature.handle_command("start", "") {
            CommandResult::Display(output) => {
                assert!(
                    output.contains("User command /start -> prompt:init"),
                    "{output}"
                );
                assert!(output.contains("Project Init"), "{output}");
                assert!(output.contains("Safety:"), "{output}");
            }
            other => panic!("unexpected command result: {other:?}"),
        }
    }

    #[test]
    fn user_command_rejects_builtin_collision() {
        let manifest = UserCommandManifest {
            name: "prompt".into(),
            description: None,
            target: "prompt:init".into(),
            mode: "preview".into(),
            availability: UserCommandAvailability::default(),
            safety: UserCommandSafety::default(),
        };
        assert!(validate_manifest(&manifest).is_err());
    }
}
